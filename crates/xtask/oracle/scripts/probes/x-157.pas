{ parity probe: x-157 }
unit P_x_157;

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
  v := '5'; v := v > 4; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
