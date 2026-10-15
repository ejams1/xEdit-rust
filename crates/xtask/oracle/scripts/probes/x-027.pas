{ parity probe: x-027 }
unit P_x_027;

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
  Abort;
  AddMessage('after');
end;
end.
