{ parity probe: x-313 }
unit P_x_313;

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
  sl.Delete(1.5);
  AddMessage('after');
end;
end.
