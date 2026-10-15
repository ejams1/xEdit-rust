{ parity probe: x-087 }
unit P_x_087;

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
  AddMessage(FloatToStr(-0.0));
  AddMessage('after');
end;
end.
