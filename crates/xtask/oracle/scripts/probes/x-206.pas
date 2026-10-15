{ parity probe: x-206 }
unit P_x_206;

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
  v := nil = 0; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
