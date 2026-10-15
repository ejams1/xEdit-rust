{ parity probe: x-143 }
unit P_x_143;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  AddMessage(IntToStr(StrToInt('1.0')));
  AddMessage('after');
end;
end.
