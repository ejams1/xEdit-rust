{ parity probe: x-304 }
unit P_x_304;

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
  sl.Duplicates := 7;
  AddMessage('after');
end;
end.
