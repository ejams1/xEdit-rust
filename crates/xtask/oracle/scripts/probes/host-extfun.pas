{
  Parity probe (issue #25): functions a script declares with `external`
  (JvInterpreter's AddExtFun of InterpretFunction, called through CallDll):
  integer, boolean and small integer results, a string and a var argument,
  a procedure, a library named by a constant, and the refusals.
}
unit HostExtFun;

const
  cKernel = 'kernel32.dll';

function MulDiv(a, b, c: Integer): Integer; external 'kernel32.dll' name 'MulDiv';
function lstrlenW(s: string): Integer; external cKernel name 'lstrlenW';
function IsCharAlphaW(ch: Integer): Boolean; external 'user32.dll' name 'IsCharAlphaW';
function MulDivWord(a, b, c: Integer): Word; external 'kernel32.dll' name 'MulDiv';
function MulDivSmall(a, b, c: Integer): SmallInt; external 'kernel32.dll' name 'MulDiv';
function BoolArg(a: Boolean; b, c: Integer): Integer; external 'kernel32.dll' name 'MulDiv';
function GetBinaryTypeW(s: string; var t: Integer): Boolean; external 'kernel32.dll' name 'GetBinaryTypeW';
procedure SleepFor25(ms: Integer); external 'kernel32.dll' name 'Sleep';
function MissingLib(a: Integer): Integer; external 'nosuchlib25.dll' name 'X';
function MissingFn(a: Integer): Integer; external 'kernel32.dll' name 'NoSuchFunction25';
function ByIndex(a: Integer): Integer; external 'kernel32.dll' index 5;

procedure Try1(aCase: Integer);
var
  v: Variant;
  t: Integer;
begin
  try
    case aCase of
      1: v := MulDiv(10, 6, 4);
      2: v := MulDiv(-7, 3, 2);
      3: v := lstrlenW('hello');
      4: v := IsCharAlphaW(65);
      5: v := IsCharAlphaW(49);
      6: v := MulDivWord(100000, 1, 1);
      7: v := MulDivSmall(-5, 1, 1);
      8: v := BoolArg(True, 2, 1);
      9: begin
           t := 99;
           v := GetBinaryTypeW('C:\Windows\System32\cmd.exe', t);
           AddMessage('  var argument ' + IntToStr(t));
         end;
      10: v := SleepFor25(1);
      11: v := MissingLib(1);
      12: v := MissingFn(1);
      13: v := ByIndex(1);
      14: v := MulDiv(1, 2);
      15: v := MulDiv('7', 2, 1);
    end;
    AddMessage(IntToStr(aCase) + ': ' + VarToStr(v) + ' (VarType ' + IntToStr(VarType(v)) + ')');
  except
    on E: Exception do
      AddMessage(IntToStr(aCase) + ': ' + E.Message);
  end;
end;

function Initialize: Integer;
var
  i: Integer;
begin
  Result := 0;
  for i := 1 to 15 do
    Try1(i);
end;

end.
