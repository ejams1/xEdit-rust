{ parity probe: x-086 }
unit P_x_086;

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
  AddMessage(FloatToStr(123456789012345678.0));
  AddMessage('after');
end;
end.
