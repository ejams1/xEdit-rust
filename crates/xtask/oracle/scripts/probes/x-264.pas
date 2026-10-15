{ parity probe: x-264 }
unit P_x_264;

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
  v := FloatToStr('1.5'); AddMessage(v);
  AddMessage('after');
end;
end.
