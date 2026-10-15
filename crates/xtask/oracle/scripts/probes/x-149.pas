{ parity probe: x-149 }
unit P_x_149;

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
  AddMessage(FloatToStr(StrToFloat('Inf')));
  AddMessage('after');
end;
end.
