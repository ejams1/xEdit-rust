{ parity probe: x-145 }
unit P_x_145;

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
  AddMessage(FloatToStr(StrToFloat('1,5')));
  AddMessage('after');
end;
end.
