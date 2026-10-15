{ parity probe: x-314 }
unit P_x_314;

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
  sl.Delete(0.5);
  AddMessage('after');
end;
end.
