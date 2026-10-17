{
  Parity probe (issue #25): an external function with a Double result.
  CallDllIns refuses the result type with ieDllInvalidResult; the
  4.1.5q release faults (an access violation) on that path instead.
}
unit HostExtFunResult;

function DoubleResult(a, b, c: Integer): Double; external 'kernel32.dll' name 'MulDiv';

function Initialize: Integer;
begin
  Result := 0;
  AddMessage('before');
  AddMessage(IntToStr(FloatToStr(DoubleResult(1, 2, 3))));
  AddMessage('after');
end;

end.
