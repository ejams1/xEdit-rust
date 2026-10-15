{ parity probe: x-263 }
unit P_x_263;

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
  v := FloatToStr('x');
  AddMessage('after');
end;
end.
